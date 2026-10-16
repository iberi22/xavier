// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

contract XavierDAO {
    // Role identifiers. DEFAULT_ADMIN_ROLE administers every other role.
    bytes32 public constant DEFAULT_ADMIN_ROLE = 0x00;
    bytes32 public constant PROPOSER_ROLE = keccak256("PROPOSER_ROLE");
    bytes32 public constant COUNCIL_ROLE = keccak256("COUNCIL_ROLE");
    bytes32 public constant VETO_ROLE = keccak256("VETO_ROLE");

    struct Proposal {
        bytes32 clusterId;
        string title;
        string description;
        uint256 userVotesYes;
        uint256 userVotesNo;
        uint256 councilVotesYes;
        uint256 councilVotesNo;
        uint256 createdAt;
        bool approved;
        bool executed;
        bool vetoed;
        string vetoReason;
    }

    mapping(bytes32 => Proposal) public proposals;
    mapping(bytes32 => mapping(address => bool)) public hasVoted;
    mapping(bytes32 => mapping(address => bool)) private _roles;

    // Registered community voting power per account, set by the admin.
    mapping(address => uint256) public votingPowerOf;

    event RoleGranted(bytes32 indexed role, address indexed account, address indexed sender);
    event RoleRevoked(bytes32 indexed role, address indexed account, address indexed sender);
    event VotingPowerSet(address indexed account, uint256 power);
    event ProposalCreated(bytes32 indexed clusterId, string title, string description);
    event VoteCast(bytes32 indexed clusterId, address indexed voter, bool approve, uint256 votingPower, bool isCouncil);
    event ProposalExecuted(bytes32 indexed clusterId, bool approved);
    event ProposalVetoed(bytes32 indexed clusterId, string reason);
    event VetoOverruled(bytes32 indexed clusterId);

    // 48 hours voting period + 24 hours timelock execution delay
    uint256 public constant VOTING_PERIOD = 48 hours;
    uint256 public constant TIMELOCK_DELAY = 24 hours;

    modifier onlyRole(bytes32 role) {
        require(_roles[role][msg.sender], "Missing role");
        _;
    }

    constructor() {
        _grantRole(DEFAULT_ADMIN_ROLE, msg.sender);
    }

    function hasRole(bytes32 role, address account) public view returns (bool) {
        return _roles[role][account];
    }

    function grantRole(bytes32 role, address account) external onlyRole(DEFAULT_ADMIN_ROLE) {
        _grantRole(role, account);
    }

    function revokeRole(bytes32 role, address account) external onlyRole(DEFAULT_ADMIN_ROLE) {
        _revokeRole(role, account);
    }

    function renounceRole(bytes32 role) external {
        _revokeRole(role, msg.sender);
    }

    function setVotingPower(address account, uint256 power) external onlyRole(DEFAULT_ADMIN_ROLE) {
        votingPowerOf[account] = power;
        emit VotingPowerSet(account, power);
    }

    function createProposal(bytes32 clusterId, string calldata title, string calldata description)
        external
        onlyRole(PROPOSER_ROLE)
    {
        require(clusterId != bytes32(0), "Invalid cluster id");
        require(proposals[clusterId].clusterId == bytes32(0), "Proposal already exists");
        proposals[clusterId] = Proposal({
            clusterId: clusterId,
            title: title,
            description: description,
            userVotesYes: 0,
            userVotesNo: 0,
            councilVotesYes: 0,
            councilVotesNo: 0,
            createdAt: block.timestamp,
            approved: false,
            executed: false,
            vetoed: false,
            vetoReason: ""
        });
        emit ProposalCreated(clusterId, title, description);
    }

    // `votingPower` and `isCouncil` are kept for ABI compatibility with the
    // off-chain client. They are not trusted: they must match on-chain state
    // for the caller, otherwise the vote reverts.
    function castVote(bytes32 clusterId, bool approve, uint256 votingPower, bool isCouncil) external {
        Proposal storage p = proposals[clusterId];
        require(p.clusterId != bytes32(0), "Proposal does not exist");
        require(block.timestamp <= p.createdAt + VOTING_PERIOD, "Voting period has ended");
        require(!hasVoted[clusterId][msg.sender], "Voter already voted");
        require(isCouncil == _roles[COUNCIL_ROLE][msg.sender], "Council flag mismatch");
        require(votingPower == votingPowerOf[msg.sender], "Voting power mismatch");

        hasVoted[clusterId][msg.sender] = true;

        if (isCouncil) {
            if (approve) {
                p.councilVotesYes += 1;
            } else {
                p.councilVotesNo += 1;
            }
        } else {
            // Community votes are weighted by the registered voting power.
            require(votingPower > 0, "No voting power");
            if (approve) {
                p.userVotesYes += votingPower;
            } else {
                p.userVotesNo += votingPower;
            }
        }

        emit VoteCast(clusterId, msg.sender, approve, votingPower, isCouncil);
    }

    function vetoProposal(bytes32 clusterId, string calldata reason) external onlyRole(VETO_ROLE) {
        Proposal storage p = proposals[clusterId];
        require(p.clusterId != bytes32(0), "Proposal does not exist");
        require(!p.executed, "Proposal already executed");

        p.vetoed = true;
        p.vetoReason = reason;
        emit ProposalVetoed(clusterId, reason);
    }

    function overruleVeto(bytes32 clusterId) external {
        Proposal storage p = proposals[clusterId];
        require(p.clusterId != bytes32(0), "Proposal does not exist");
        require(p.vetoed, "Proposal is not vetoed");
        require(!p.executed, "Proposal already executed");
        // Only participants of this proposal can trigger an overrule. The
        // 75% community threshold below is still enforced.
        require(hasVoted[clusterId][msg.sender], "Only voters can overrule");

        // Requires 75% of community (user) votes to overrule a veto
        uint256 totalUserVotes = p.userVotesYes + p.userVotesNo;
        require(totalUserVotes > 0, "No user votes cast");
        require((p.userVotesYes * 100) >= (totalUserVotes * 75), "Overrule threshold not reached");

        p.vetoed = false;
        emit VetoOverruled(clusterId);
    }

    function getProposalStatus(bytes32 clusterId) external view returns (
        bool approved,
        uint256 userVotesYes,
        uint256 userVotesNo,
        uint256 councilVotesYes,
        uint256 councilVotesNo,
        bool vetoed,
        bool executed
    ) {
        Proposal memory p = proposals[clusterId];
        return (
            p.approved,
            p.userVotesYes,
            p.userVotesNo,
            p.councilVotesYes,
            p.councilVotesNo,
            p.vetoed,
            p.executed
        );
    }

    function executeProposal(bytes32 clusterId) external {
        Proposal storage p = proposals[clusterId];
        require(p.clusterId != bytes32(0), "Proposal does not exist");
        require(!p.executed, "Proposal already executed");
        require(!p.vetoed, "Proposal is vetoed");
        require(block.timestamp >= p.createdAt + VOTING_PERIOD + TIMELOCK_DELAY, "Timelock delay not met");

        // Quorum and Threshold verification
        // Users Quorum: at least 100 XP total voting power
        // Council Quorum: at least 2 council votes cast
        uint256 totalUserVotes = p.userVotesYes + p.userVotesNo;
        uint256 totalCouncilVotes = p.councilVotesYes + p.councilVotesNo;

        bool userPassed = totalUserVotes >= 100 && (p.userVotesYes * 100) > (totalUserVotes * 50);
        bool councilPassed = totalCouncilVotes >= 2 && (p.councilVotesYes * 100) > (totalCouncilVotes * 50);

        if (userPassed && councilPassed) {
            p.approved = true;
        } else {
            p.approved = false;
        }

        p.executed = true;
        emit ProposalExecuted(clusterId, p.approved);
    }

    function _grantRole(bytes32 role, address account) internal {
        if (!_roles[role][account]) {
            _roles[role][account] = true;
            emit RoleGranted(role, account, msg.sender);
        }
    }

    function _revokeRole(bytes32 role, address account) internal {
        if (_roles[role][account]) {
            _roles[role][account] = false;
            emit RoleRevoked(role, account, msg.sender);
        }
    }
}
