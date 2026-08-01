// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

contract XavierDAO {
    struct Proposal {
        bytes32 clusterId;
        string title;
        string description;
        uint64 upvotes;
        uint64 downvotes;
        bool approved;
        bool executed;
    }

    mapping(bytes32 => Proposal) public proposals;
    mapping(bytes32 => mapping(address => bool)) public hasVoted;

    event ProposalCreated(bytes32 indexed clusterId, string title, string description);
    event VoteCast(bytes32 indexed clusterId, address indexed voter, bool approve);
    event ProposalExecuted(bytes32 indexed clusterId, bool approved);

    function createProposal(bytes32 clusterId, string calldata title, string calldata description) external {
        require(proposals[clusterId].clusterId == bytes32(0), "Proposal already exists");
        proposals[clusterId] = Proposal({
            clusterId: clusterId,
            title: title,
            description: description,
            upvotes: 0,
            downvotes: 0,
            approved: false,
            executed: false
        });
        emit ProposalCreated(clusterId, title, description);
    }

    function castVote(bytes32 clusterId, bool approve) external {
        require(proposals[clusterId].clusterId != bytes32(0), "Proposal does not exist");
        require(!hasVoted[clusterId][msg.sender], "Voter already voted");

        hasVoted[clusterId][msg.sender] = true;
        if (approve) {
            proposals[clusterId].upvotes += 1;
        } else {
            proposals[clusterId].downvotes += 1;
        }

        emit VoteCast(clusterId, msg.sender, approve);
    }

    function getProposalStatus(bytes32 clusterId) external view returns (bool approved, uint64 upvotes, uint64 downvotes) {
        Proposal memory p = proposals[clusterId];
        return (p.approved, p.upvotes, p.downvotes);
    }

    function executeProposal(bytes32 clusterId) external {
        require(proposals[clusterId].clusterId != bytes32(0), "Proposal does not exist");
        Proposal storage p = proposals[clusterId];
        require(!p.executed, "Proposal already executed");

        uint64 total = p.upvotes + p.downvotes;
        if (total >= 5 && (p.upvotes * 100) >= (total * 80)) {
            p.approved = true;
        } else if (total > 0 && p.upvotes > p.downvotes) {
            p.approved = true;
        } else {
            p.approved = false;
        }

        p.executed = true;
        emit ProposalExecuted(clusterId, p.approved);
    }
}
